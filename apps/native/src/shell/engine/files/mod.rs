//! Projects on disk: New, Open, Save, Save as, autosave, recent files. A `.ppstudio` file
//! (`crates/ppstudio`) maps to and from the engine (layers, drawings, the sheet, palette…).
//!
//! - [`load`]: a file into the engine; [`save`]: the engine into a file (only changed
//!   tiles encoded, the rest copied from the file as stored);
//! - this file: the File menu's actions, the system's file dialog, "Save first?", whether
//!   there are unsaved changes, the recovery copy every two minutes, recent files.
//!
//! Where things go: the recent files in `$XDG_CONFIG_HOME/pencilpilot/recent` (one path a
//! line, newest first); the recovery copy in
//! `$XDG_STATE_HOME/pencilpilot/recovery/<name>-<hash of its path>.ppstudio` (else
//! `~/.config`, `~/.local/state`), deleted once the project is saved or its changes let go.

pub mod load;
pub mod popup;
mod recent;
pub mod save;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use brush::Painter;
use pencilpilot_ui::ui::{Kind, Rect};

pub use popup::Question;

use super::Engine;
use super::frames::{Cel, Mark, Sheet, Track};
use crate::shell::dialog::{self, Ask, Asked};
use crate::state::{Action, ColourStatus, Layer, Note, State};

/// The File menu's items this part carries out (the header pushes them as [`Action::File`]).
pub const NEW: &str = "New…";
pub const OPEN: &str = "Open…";
pub const SAVE: &str = "Save";
pub const SAVE_AS: &str = "Save as…";
/// A recent file picked: this, then its path.
pub const OPEN_RECENT: &str = "Open recent: ";
/// "Save first?"'s answers.
pub const CONFIRM_SAVE: &str = "Save first: Save";
pub const CONFIRM_DISCARD: &str = "Save first: Don't save";
pub const CONFIRM_CANCEL: &str = "Save first: Cancel";
/// The File menu's Quit (Ctrl+Q), and the window's close button.
pub const QUIT: &str = "Quit";
/// "Recover your unsaved changes?"'s answers.
pub const RECOVER_OPEN: &str = "Recovery: Open";
pub const RECOVER_DISCARD: &str = "Recovery: Discard";
pub const RECOVER_LATER: &str = "Recovery: Later";

/// A recovery copy is written this long after the first unsaved change, and again as long
/// after each.
pub const AUTOSAVE: Duration = Duration::from_secs(120);

const FILTER: &str = "PencilPilot project";

/// The document open: what saving needs from opening it.
pub struct Doc {
    /// the file it was opened from or last saved to, kept open: a save copies from it
    /// what it doesn't change
    pub source: Option<ppstudio::Opened<std::io::BufReader<std::fs::File>>>,
    /// that file's manifest (what the native app doesn't edit is kept from it)
    pub base: Option<ppstudio::Manifest>,
    /// the scene and cut open
    pub scene: usize,
    pub cut: usize,
    pub scene_name: String,
    /// the file has scenes (else its frames were read as one cut)
    pub had_scenes: bool,
    /// the painter's layer ids → the file's
    pub layer_ids: HashMap<u32, u32>,
}

impl Default for Doc {
    fn default() -> Self {
        Doc {
            source: None,
            base: None,
            scene: 0,
            cut: 0,
            scene_name: "Scene 1".into(),
            had_scenes: false,
            layer_ids: HashMap::new(),
        }
    }
}

impl std::fmt::Debug for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Doc")
            .field("source", &self.source.is_some())
            .field("scene", &self.scene)
            .field("cut", &self.cut)
            .finish_non_exhaustive()
    }
}

/// What to do once "Save first?" is answered (and the save, if any, done).
#[derive(Debug, Clone, PartialEq)]
pub enum Then {
    New,
    Open,
    OpenPath(PathBuf),
    Quit,
}

/// What the file dialog was asked for.
#[derive(Debug)]
enum Asking {
    Open,
    SaveAs(Option<Then>),
}

/// The sheet and settings as last saved (unsaved changes: they differ, or the painter's
/// history moved).
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    layers: Vec<Layer>,
    tracks: Vec<(char, f32, Vec<Cel>)>,
    status: std::collections::BTreeMap<u32, ColourStatus>,
    loop_range: Option<(u32, u32)>,
    fps: u32,
    palette: (String, Vec<(String, [f32; 4])>),
    notes: Vec<Note>,
    cut: String,
}

impl Snapshot {
    fn of(s: &State) -> Snapshot {
        Snapshot {
            layers: s.layers.clone(),
            tracks: s
                .sheet
                .tracks
                .iter()
                .map(|t| (t.kind, t.opacity, t.cels.clone()))
                .collect(),
            status: s.sheet.status.clone(),
            loop_range: s.sheet.loop_range,
            fps: s.fps,
            palette: (s.palette_name.clone(), s.palette.clone()),
            notes: s.notes.clone(),
            cut: s.cut_name.clone(),
        }
    }
}

/// What the engine keeps about the open project between calls.
#[derive(Debug, Default)]
pub struct Files {
    pub doc: Doc,
    dialog: Option<(Asking, Asked)>,
    /// what "Save first?" is asked for
    then: Option<Then>,
    /// the painter's history count and the sheet when last saved (or opened)
    saved_edits: u64,
    saved: Option<Snapshot>,
    /// when changes were first left unsaved, and the last recovery copy
    unsaved_since: Option<Instant>,
    autosaved: Option<Instant>,
    /// the recent files were read
    started: bool,
    /// how the last open and save went (for checks)
    pub last_open: Option<load::OpenTimes>,
    pub last_save: Option<save::SaveTimes>,
    /// a save being written on another thread, and the next one asked for meanwhile
    saving: Option<Saving>,
    queued: Option<(PathBuf, Option<Then>)>,
    /// a recovery copy offered, and the file it's a copy of (none: never saved)
    offer: Option<(PathBuf, Option<PathBuf>)>,
}

/// A save being written on another thread.
#[derive(Debug)]
struct Saving {
    rx: std::sync::mpsc::Receiver<Written>,
    path: PathBuf,
    /// a recovery copy (not the project's file)
    recovery: bool,
    then: Option<Then>,
    /// the history count and the sheet it saved (changes made since stay unsaved)
    edits: u64,
    snapshot: Snapshot,
    doc_id: u32,
    /// how long the window's part took (ms), and when it started
    times: save::SaveTimes,
    started: Instant,
}

/// What the writing thread hands back.
struct Written {
    result: Result<(), String>,
    /// the file the save copied from (back for the next save)
    source: Option<Source>,
    manifest: Option<ppstudio::Manifest>,
}

impl std::fmt::Debug for Written {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Written")
            .field("result", &self.result)
            .finish_non_exhaustive()
    }
}

type Source = ppstudio::Opened<std::io::BufReader<std::fs::File>>;

/// What the interface shows of the project (in [`State::project`]): its name, path, whether
/// it has unsaved changes, recent files…
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectUi {
    /// the project's name (its file's, without `.ppstudio`)
    pub name: String,
    /// its file (none: never saved)
    pub path: Option<PathBuf>,
    /// the scene open
    pub scene: String,
    pub unsaved: bool,
    /// recent files, newest first (at most ten)
    pub recent: Vec<PathBuf>,
    /// a question showing (Save first?, Recover?)
    pub asking: Option<Question>,
    /// a save being written
    pub saving: bool,
    /// what went wrong last (shown under the title until the next file action)
    pub note: String,
    /// where the last recovery copy went
    pub recovery: Option<PathBuf>,
    /// a file to open once the engine is up (`pencilpilot path.ppstudio`)
    pub startup: Option<PathBuf>,
    /// where the question's buttons are, when it shows (in its `buttons` order)
    pub ask_buttons: Vec<Rect>,
}

impl Default for ProjectUi {
    /// The demo's project (the design's).
    fn default() -> Self {
        ProjectUi {
            name: "Ember".into(),
            path: None,
            scene: "Scene 1".into(),
            unsaved: false,
            recent: Vec::new(),
            asking: None,
            note: String::new(),
            recovery: None,
            startup: None,
            ask_buttons: Vec::new(),
            saving: false,
        }
    }
}

impl ProjectUi {
    /// The header's line under the title: "Scene 1 · Cut 2 · saved", or what went wrong.
    pub fn subtitle(&self, cut: &str) -> String {
        if !self.note.is_empty() {
            return self.note.clone();
        }
        let saved = if self.saving {
            "saving…"
        } else if self.unsaved {
            "unsaved"
        } else {
            "saved"
        };
        format!("{} · {cut} · {saved}", self.scene)
    }
}

/// The palette a project without one starts with.
pub fn default_palette() -> (String, Vec<(String, [f32; 4])>) {
    use pencilpilot_ui::theme::hex;
    let c = [
        ("Ink", 0x16161A),
        ("Grey", 0xA9A8AE),
        ("Paper", 0xF6F4EF),
        ("Red", 0xE8492F),
        ("Yellow", 0xFFD84D),
        ("Green", 0x5FD08A),
        ("Blue", 0x3D6BFF),
        ("Purple", 0x7C6CF0),
    ];
    (
        "Palette".into(),
        c.iter().map(|(n, v)| ((*n).into(), hex(*v))).collect(),
    )
}

/// Review notes as the native app keeps them in `extra`.
pub fn notes_json(notes: &[Note]) -> serde_json::Value {
    serde_json::Value::Array(
        notes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "author": n.author,
                    "text": n.text,
                    "frame": n.frame,
                    "colour": load::colour_hex(n.colour),
                    "resolved": n.resolved,
                })
            })
            .collect(),
    )
}

pub fn notes_from(extra: &serde_json::Value) -> Vec<Note> {
    let Some(a) = extra.get("notes").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    a.iter()
        .filter_map(|n| {
            Some(Note {
                author: n.get("author")?.as_str()?.into(),
                text: n.get("text")?.as_str()?.into(),
                frame: n.get("frame")?.as_u64()? as u32,
                colour: n
                    .get("colour")
                    .and_then(|c| c.as_str())
                    .and_then(load::hex_colour)
                    .unwrap_or(pencilpilot_ui::theme::hex(0xE8492F)),
                resolved: n.get("resolved").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        })
        .collect()
}

/// The canvas tools' interface state for a new document: nothing selected or moving, the
/// canvas menu's settings kept.
pub fn fresh_tooling(t: &super::tools::ToolsUi) -> super::tools::ToolsUi {
    super::tools::ToolsUi {
        shape_kind: t.shape_kind,
        grid: t.grid,
        snap: t.snap,
        symmetry: t.symmetry,
        timelapse: t.timelapse,
        ..Default::default()
    }
}

/// Puts down what's being moved, so the painter holds every pixel and the sheet every
/// drawing (before saving).
fn settle(e: &mut Engine, s: &mut State) {
    if e.is_stroking() {
        e.up();
    }
    if e.float.is_some() || e.painter.transforming() {
        super::tools::apply(e, s, &Action::Other("Transform: commit".into()));
        if e.float.is_some() {
            e.float_end(true);
        } else if e.painter.transforming() {
            e.painter.transform_commit();
        }
    }
    // (a drawing just started on a blank frame joins the sheet)
    super::frames::tick(e, s, Instant::now());
}

/// A File action (New, Open…, Save…). Returns whether it was one and the picture may have
/// changed.
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    let Action::File(item) = a else {
        return false;
    };
    match item.as_str() {
        NEW => {
            s.project.note.clear();
            first(e, s, Then::New);
        }
        OPEN => {
            s.project.note.clear();
            first(e, s, Then::Open);
        }
        x if x.starts_with(OPEN_RECENT) => {
            s.project.note.clear();
            let p = PathBuf::from(&x[OPEN_RECENT.len()..]);
            first(e, s, Then::OpenPath(p));
        }
        SAVE => {
            s.project.note.clear();
            save_then(e, s, None);
        }
        SAVE_AS => {
            s.project.note.clear();
            ask_save_as(e, s, None);
        }
        CONFIRM_SAVE => {
            s.project.asking = None;
            if let Some(t) = e.files.then.take() {
                save_then(e, s, Some(t));
            }
        }
        CONFIRM_DISCARD => {
            s.project.asking = None;
            if let Some(t) = e.files.then.take() {
                drop_recovery(s);
                carry_out(e, s, t);
            }
        }
        CONFIRM_CANCEL => {
            s.project.asking = None;
            e.files.then = None;
        }
        QUIT => {
            if s.project.asking.is_none() {
                first(e, s, Then::Quit);
            }
        }
        RECOVER_OPEN => {
            s.project.asking = None;
            if let Some((rec, of)) = e.files.offer.take() {
                recover(e, s, &rec, of);
            }
        }
        RECOVER_DISCARD => {
            s.project.asking = None;
            if let Some((rec, _)) = e.files.offer.take() {
                let _ = std::fs::remove_file(rec.with_extension("path"));
                let _ = std::fs::remove_file(rec);
            }
        }
        RECOVER_LATER => {
            s.project.asking = None;
            e.files.offer = None;
        }
        _ => return false,
    }
    true
}

/// `then`, asking "Save first?" when there are unsaved changes.
fn first(e: &mut Engine, s: &mut State, then: Then) {
    if e.files.dialog.is_some() || s.project.asking.is_some() {
        return;
    }
    if s.project.unsaved {
        let title = if then == Then::Quit {
            format!("Save the changes to {} before quitting?", s.project.name)
        } else {
            format!("Save the changes to {} first?", s.project.name)
        };
        s.project.asking = Some(Question {
            title,
            detail: "Your changes are lost if you don’t save them.".into(),
            buttons: vec![
                ("Save".into(), CONFIRM_SAVE.into(), Kind::Primary),
                ("Don’t save".into(), CONFIRM_DISCARD.into(), Kind::Secondary),
                ("Cancel".into(), CONFIRM_CANCEL.into(), Kind::Ghost),
            ],
            dismiss: CONFIRM_CANCEL.into(),
        });
        e.files.then = Some(then);
    } else {
        carry_out(e, s, then);
    }
}

fn carry_out(e: &mut Engine, s: &mut State, then: Then) {
    match then {
        Then::New => new(e, s),
        Then::Open => {
            if e.files.dialog.is_none() {
                let a = dialog::ask(Ask {
                    title: "Open a project".into(),
                    save: false,
                    name: None,
                    filters: vec![(FILTER.into(), vec!["*.ppstudio".into()])],
                });
                e.files.dialog = Some((Asking::Open, a));
            }
        }
        Then::OpenPath(p) => {
            open_path(e, s, &p);
        }
        // (a save still being written finishes first)
        Then::Quit => match &mut e.files.saving {
            Some(sv) if sv.then.is_none() => sv.then = Some(Then::Quit),
            Some(_) => {}
            None => s.quit_now = true,
        },
    }
}

/// The window asks to close: at once with nothing unsaved, else "Save first?" (Save:
/// closes once saved; Don't save: closes; Cancel: stays). [`State::quit_now`] says when.
pub fn quit_requested(e: &mut Engine, s: &mut State) {
    apply(e, s, &Action::File(QUIT.into()));
}

/// Save to the project's file (Save as… when it has none), then `then`.
fn save_then(e: &mut Engine, s: &mut State, then: Option<Then>) {
    match s.project.path.clone() {
        Some(p) => start_save(e, s, &p, then),
        None => ask_save_as(e, s, then),
    }
}

fn ask_save_as(e: &mut Engine, s: &State, then: Option<Then>) {
    if e.files.dialog.is_some() {
        return;
    }
    let a = dialog::ask(Ask {
        title: "Save the project".into(),
        save: true,
        name: Some(format!("{}.ppstudio", s.project.name)),
        filters: vec![(FILTER.into(), vec!["*.ppstudio".into()])],
    });
    e.files.dialog = Some((Asking::SaveAs(then), a));
}

/// The document replaced: a new painter, and the frames and tools set up afresh for it.
pub(super) fn replace(e: &mut Engine, s: &mut State, painter: Painter) {
    s.view.doc_w = painter.width();
    s.view.doc_h = painter.height();
    e.painter = painter;
    e.frames = Default::default();
    e.tools = Default::default();
    e.float = None;
    e.stroking = None;
    e.tap = None;
    s.moving = None;
    s.playing = false;
    s.tooling = fresh_tooling(&s.tooling);
    s.doc_id = s.doc_id.wrapping_add(1);
}

/// What's open now is what's saved.
fn mark_saved(e: &mut Engine, s: &mut State) {
    e.files.saved_edits = e.painter.history_changes();
    e.files.saved = Some(Snapshot::of(s));
    e.files.unsaved_since = None;
    e.files.autosaved = None;
    s.project.unsaved = false;
}

/// A new, empty project: 1920 × 1080, 24 fps, one layer, one empty cel.
pub fn new(e: &mut Engine, s: &mut State) {
    replace(e, s, Painter::new(1920, 1080));
    s.layers = vec![Layer {
        name: "Layer 1".into(),
        visible: true,
        locked: false,
        kind: 'C',
    }];
    let mut t = Track::new(0, 'C');
    t.cels.push(Cel::new(0, 1, Mark::None));
    s.sheet = Sheet {
        tracks: vec![t],
        ..Default::default()
    };
    s.frames = 1;
    s.frame = 0;
    s.fps = 24;
    s.active_layer = 0;
    s.cut_name = "Cut 1".into();
    (s.palette_name, s.palette) = default_palette();
    s.notes.clear();
    super::frames::derive(s);
    super::frames::tick(e, s, Instant::now());
    e.painter.clear_history();
    e.files.doc = Doc::default();
    s.project.name = "Untitled".into();
    s.project.path = None;
    s.project.scene = "Scene 1".into();
    s.project.recovery = None;
    mark_saved(e, s);
}

/// Opens `path` (an error shows under the title; nothing else changes then).
pub fn open_path(e: &mut Engine, s: &mut State, path: &Path) -> bool {
    open_file(e, s, path, true)
}

fn open_file(e: &mut Engine, s: &mut State, path: &Path, listed: bool) -> bool {
    match load::open(e, s, path) {
        Ok((doc, times)) => {
            s.project.scene = doc.scene_name.clone();
            e.files.doc = doc;
            e.files.last_open = Some(times);
            s.project.name = stem(path);
            s.project.path = Some(path.to_path_buf());
            s.project.note.clear();
            s.project.recovery = None;
            mark_saved(e, s);
            if listed {
                s.project.recent = recent::add(path);
            }
            true
        }
        Err(err) => {
            s.project.note = format!("Couldn’t open {}: {err}", stem(path));
            false
        }
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned())
}

/// Starts saving to `path` (it's the project's file once saved), then `then`. What must
/// be read from the painter is read now (only changed tiles: fast); the file is written on
/// another thread ([`tick`] takes the answer). A save asked for while one is written
/// waits for it (the last one asked for).
pub fn start_save(e: &mut Engine, s: &mut State, path: &Path, then: Option<Then>) {
    if e.files.saving.is_some() {
        e.files.queued = Some((path.to_path_buf(), then));
        return;
    }
    write_in_background(e, s, path, then, false);
}

/// The window's part of a save (or a recovery copy): the project made; its writing handed
/// to another thread.
fn write_in_background(
    e: &mut Engine,
    s: &mut State,
    path: &Path,
    then: Option<Then>,
    recovery: bool,
) {
    let started = Instant::now();
    settle(e, s);
    let mut doc = std::mem::take(&mut e.files.doc);
    let made = save::project(e, s, &mut doc);
    // (the painter's own picture catches up with what the GPU composited)
    super::frames::catch_up(e, s);
    e.painter.flush();
    let thumb = save::thumbnail(e);
    let source = doc.source.take();
    e.files.doc = doc;
    let (mut p, times) = match made {
        Ok(x) => x,
        Err(err) => {
            e.files.doc.source = source;
            s.project.note = format!("Couldn’t save: {err}");
            return;
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let to = path.to_path_buf();
    let original = s.project.path.clone();
    std::thread::spawn(move || {
        p.thumbnail = save::png(&thumb);
        let mut source = source;
        let result = p.save_from(&to, source.as_mut()).map_err(|e| e.to_string());
        if recovery {
            // (which file it's a copy of: for offering it at the next start)
            let of = original.map(|p| p.to_string_lossy().into_owned());
            let _ = std::fs::write(to.with_extension("path"), of.unwrap_or_default());
        }
        // saved: the next save copies from the new file
        let source = match (&result, recovery) {
            (Ok(()), false) => std::fs::File::open(&to)
                .ok()
                .and_then(|f| ppstudio::Opened::read(std::io::BufReader::new(f)).ok())
                .or(source),
            _ => source,
        };
        let _ = tx.send(Written {
            result,
            source,
            manifest: Some(p.manifest),
        });
    });
    e.files.saving = Some(Saving {
        rx,
        path: path.to_path_buf(),
        recovery,
        then,
        edits: e.painter.history_changes(),
        snapshot: Snapshot::of(s),
        doc_id: s.doc_id,
        times: save::SaveTimes {
            window_ms: started.elapsed().as_secs_f64() * 1e3,
            ..times
        },
        started,
    });
    s.project.saving = !recovery;
}

/// A save written: the project's file from now on (or why not, under the title).
fn finish_save(e: &mut Engine, s: &mut State, sv: Saving, w: Written) -> bool {
    s.project.saving = false;
    if sv.doc_id != s.doc_id {
        // (another document was opened meanwhile)
        return w.result.is_ok();
    }
    if w.source.is_some() {
        e.files.doc.source = w.source;
    }
    let mut times = sv.times;
    times.write_ms = sv.started.elapsed().as_secs_f64() * 1e3 - times.window_ms;
    e.files.last_save = Some(times);
    match w.result {
        Ok(()) if sv.recovery => {
            s.project.recovery = Some(sv.path);
            true
        }
        Ok(()) => {
            if w.manifest.is_some() {
                e.files.doc.base = w.manifest;
            }
            drop_recovery(s);
            s.project.name = stem(&sv.path);
            s.project.path = Some(sv.path.clone());
            s.project.recent = recent::add(&sv.path);
            // what was saved is what it had when the save started
            e.files.saved_edits = sv.edits;
            e.files.saved = Some(sv.snapshot);
            e.files.unsaved_since = None;
            e.files.autosaved = None;
            s.project.unsaved = false;
            if let Some(t) = sv.then {
                carry_out(e, s, t);
            }
            true
        }
        Err(err) => {
            s.project.note = format!("Couldn’t save: {err}");
            false
        }
    }
}

/// A save written by now, taken (waiting `wait` at most for it).
fn take_written(e: &mut Engine, s: &mut State, wait: Option<Duration>) -> Option<bool> {
    let sv = e.files.saving.as_ref()?;
    let w = match wait {
        Some(d) => sv.rx.recv_timeout(d).ok(),
        None => sv.rx.try_recv().ok(),
    };
    let w = match w {
        Some(w) => w,
        None if matches!(
            sv.rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ) =>
        {
            Written {
                result: Err("the save stopped".into()),
                source: None,
                manifest: None,
            }
        }
        None => return None,
    };
    let sv = e.files.saving.take()?;
    let ok = finish_save(e, s, sv, w);
    if let Some((p, then)) = e.files.queued.take() {
        start_save(e, s, &p, then);
    }
    Some(ok)
}

/// Waits for a save being written, and takes it (for checks).
pub fn wait_for_save(e: &mut Engine, s: &mut State) {
    while e.files.saving.is_some() {
        take_written(e, s, Some(Duration::from_secs(60)));
    }
}

/// Saves to `path` and waits for it (for checks; the interface saves with
/// [`start_save`]). False (and why, under the title) if it couldn't.
pub fn save_to(e: &mut Engine, s: &mut State, path: &Path) -> bool {
    while e.files.saving.is_some() {
        take_written(e, s, Some(Duration::from_secs(60)));
    }
    write_in_background(e, s, path, None, false);
    take_written(e, s, Some(Duration::from_secs(600))).unwrap_or(false)
}

/// Where the open project's recovery copy goes.
pub fn recovery_path(s: &State) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.project.path.hash(&mut h);
    let name: String = s
        .project
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    recent::state_dir()
        .join("recovery")
        .join(format!("{name}-{:016x}.ppstudio", h.finish()))
}

fn drop_recovery(s: &mut State) {
    if let Some(p) = s.project.recovery.take() {
        let _ = std::fs::remove_file(p.with_extension("path"));
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
thread_local! {
    /// (checks: only the one about it is offered recovery copies; checks run side by side)
    pub static OFFER_RECOVERY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn offers_recovery() -> bool {
    #[cfg(test)]
    return OFFER_RECOVERY.with(|o| o.get());
    #[cfg(not(test))]
    true
}

/// The recovery copy, written on another thread (never over the project's own file).
fn autosave(e: &mut Engine, s: &mut State) {
    let path = recovery_path(s);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    write_in_background(e, s, &path, None, true);
}

/// The newest recovery copy newer than its file (or of a project never saved), to offer.
fn find_recovery() -> Option<(PathBuf, Option<PathBuf>)> {
    let dir = recent::state_dir().join("recovery");
    let mut found: Vec<(std::time::SystemTime, PathBuf, Option<PathBuf>)> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let rec = entry.path();
        if rec.extension().is_none_or(|x| x != "ppstudio") {
            continue;
        }
        let Ok(when) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let of = std::fs::read_to_string(rec.with_extension("path"))
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(|p| PathBuf::from(p.trim()));
        let newer = match &of {
            Some(p) => std::fs::metadata(p)
                .and_then(|m| m.modified())
                .map_or(true, |t| when > t),
            None => true,
        };
        if newer {
            found.push((when, rec, of));
        }
    }
    found.sort_by_key(|(t, ..)| *t);
    found.pop().map(|(_, r, o)| (r, o))
}

/// Opens a recovery copy as the project it's a copy of, its changes unsaved (a save goes
/// to that project's file; the copy goes once saved).
fn recover(e: &mut Engine, s: &mut State, rec: &Path, of: Option<PathBuf>) {
    if !open_file(e, s, rec, false) {
        return;
    }
    s.project.name = of.as_deref().map_or_else(|| "Untitled".into(), stem);
    s.project.path = of;
    s.project.recovery = Some(rec.to_path_buf());
    s.project.unsaved = true;
    e.files.unsaved_since = Some(Instant::now());
}

/// Once a loop: answers from the file dialog, unsaved changes, autosave. Returns whether
/// the interface changed.
pub fn tick(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    let mut changed = false;
    if !e.files.started {
        e.files.started = true;
        s.project.recent = recent::load();
        // changes not saved last time, kept in a recovery copy: offered
        if let Some((rec, of)) = find_recovery().filter(|_| offers_recovery()) {
            let name = of.as_deref().map_or_else(|| "Untitled".into(), stem);
            let when = std::fs::metadata(&rec)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map_or_else(String::new, |d| {
                    let m = d.as_secs() / 60;
                    match m {
                        0 => " a moment ago".into(),
                        1..=59 => format!(" {m} min ago"),
                        _ => format!(" {} h ago", m / 60),
                    }
                });
            s.project.asking = Some(Question {
                title: format!("Recover your unsaved changes to {name}?"),
                detail: format!("A copy was kept{when}, newer than the project’s file."),
                buttons: vec![
                    (
                        "Open the recovered copy".into(),
                        RECOVER_OPEN.into(),
                        Kind::Primary,
                    ),
                    ("Discard".into(), RECOVER_DISCARD.into(), Kind::Secondary),
                ],
                dismiss: RECOVER_LATER.into(),
            });
            e.files.offer = Some((rec, of));
        }
        changed = true;
    }
    // a save written
    if take_written(e, s, None).is_some() {
        changed = true;
    }
    if let Some(p) = s.project.startup.take() {
        open_path(e, s, &p);
        changed = true;
    }
    // the file dialog answered
    if let Some(ans) = e.files.dialog.as_ref().and_then(|(_, a)| a.answer())
        && let Some((what, _)) = e.files.dialog.take()
    {
        match (what, ans) {
            (Asking::Open, Some(p)) => {
                open_path(e, s, &p);
            }
            (Asking::SaveAs(then), Some(mut p)) => {
                if p.extension().is_none_or(|x| x != "ppstudio") {
                    p.as_mut_os_string().push(".ppstudio");
                }
                start_save(e, s, &p, then);
            }
            // cancelled
            (_, None) => {}
        }
        changed = true;
    }
    // unsaved changes: the painter's history moved, or the sheet changed
    let ready = !s.sheet.tracks.is_empty() && s.sheet.tracks.iter().all(|t| t.layer != 0);
    if e.files.saved.is_none() {
        if ready {
            mark_saved(e, s);
        }
    } else if !s.project.unsaved
        && let Some(saved) = &e.files.saved
        && (e.painter.history_changes() != e.files.saved_edits || *saved != Snapshot::of(s))
    {
        s.project.unsaved = true;
        e.files.unsaved_since = Some(Instant::now());
        changed = true;
    }
    // the recovery copy
    if let Some(at) = autosave_at(e, s)
        && now >= at
        && e.files.saving.is_none()
        && !e.is_stroking()
        && e.float.is_none()
        && !e.painter.transforming()
    {
        autosave(e, s);
        e.files.autosaved = Some(Instant::now());
    }
    changed
}

fn autosave_at(e: &Engine, s: &State) -> Option<Instant> {
    if !s.project.unsaved {
        return None;
    }
    let from = match (e.files.unsaved_since, e.files.autosaved) {
        (Some(a), Some(b)) => a.max(b),
        (a, b) => a.or(b)?,
    };
    Some(from + AUTOSAVE)
}

/// When [`tick`] next has something to do (a dialog's answer, autosave).
pub fn next_tick_at(e: &Engine, s: &State) -> Option<Instant> {
    let now = Instant::now();
    if !e.files.started || s.project.startup.is_some() {
        return Some(now);
    }
    let dialog = e
        .files
        .dialog
        .as_ref()
        .map(|_| now + Duration::from_millis(50));
    let saving = e
        .files
        .saving
        .as_ref()
        .map(|_| now + Duration::from_millis(10));
    [dialog, saving, autosave_at(e, s)]
        .into_iter()
        .flatten()
        .min()
}
