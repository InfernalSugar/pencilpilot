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
mod recent;
pub mod save;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use brush::Painter;
use pencilpilot_ui::ui::{Frame, Kind, Rect};

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
}

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
    /// "Save first?" is showing: its question
    pub asking: Option<String>,
    /// what went wrong last (shown under the title until the next file action)
    pub note: String,
    /// where the last recovery copy went
    pub recovery: Option<PathBuf>,
    /// a file to open once the engine is up (`pencilpilot path.ppstudio`)
    pub startup: Option<PathBuf>,
    /// where "Save first?"'s buttons are (Save, Don't save, Cancel), when it shows
    pub ask_buttons: [Rect; 3],
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
            ask_buttons: [Rect::default(); 3],
        }
    }
}

impl ProjectUi {
    /// The header's line under the title: "Scene 1 · Cut 2 · saved", or what went wrong.
    pub fn subtitle(&self, cut: &str) -> String {
        if !self.note.is_empty() {
            return self.note.clone();
        }
        let saved = if self.unsaved { "unsaved" } else { "saved" };
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
        s.project.asking = Some(format!("Save the changes to {} first?", s.project.name));
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
    }
}

/// Save to the project's file (Save as… when it has none), then `then`.
fn save_then(e: &mut Engine, s: &mut State, then: Option<Then>) {
    match s.project.path.clone() {
        Some(p) => {
            if save_to(e, s, &p)
                && let Some(t) = then
            {
                carry_out(e, s, t);
            }
        }
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
            s.project.recent = recent::add(path);
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

/// Saves to `path`; it's the project's file from then on. False (and why, under the
/// title) if it couldn't.
pub fn save_to(e: &mut Engine, s: &mut State, path: &Path) -> bool {
    settle(e, s);
    let mut doc = std::mem::take(&mut e.files.doc);
    let res = save::write(e, s, &mut doc, path);
    let ok = match res {
        Ok((p, times)) => {
            match std::fs::File::open(path)
                .map_err(|e| e.to_string())
                .and_then(|f| {
                    ppstudio::Opened::read(std::io::BufReader::new(f)).map_err(|e| e.to_string())
                }) {
                Ok(o) => {
                    doc.source = Some(o);
                    doc.base = Some(p.manifest);
                }
                Err(err) => s.project.note = format!("Saved, but couldn’t read it back: {err}"),
            }
            e.files.last_save = Some(times);
            true
        }
        Err(err) => {
            s.project.note = format!("Couldn’t save: {err}");
            false
        }
    };
    e.files.doc = doc;
    if ok {
        drop_recovery(s);
        s.project.name = stem(path);
        s.project.path = Some(path.to_path_buf());
        mark_saved(e, s);
        s.project.recent = recent::add(path);
    }
    ok
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
        let _ = std::fs::remove_file(p);
    }
}

/// The recovery copy, written now (never over the project's own file).
fn autosave(e: &mut Engine, s: &mut State) {
    let path = recovery_path(s);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut doc = std::mem::take(&mut e.files.doc);
    let res = save::project(e, s, &mut doc).and_then(|(p, _)| {
        p.save_from(&path, doc.source.as_mut())
            .map_err(|e| e.to_string())
    });
    e.files.doc = doc;
    match res {
        Ok(()) => s.project.recovery = Some(path),
        Err(err) => s.project.note = format!("Couldn’t keep a recovery copy: {err}"),
    }
}

/// Once a loop: answers from the file dialog, unsaved changes, autosave. Returns whether
/// the interface changed.
pub fn tick(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    let mut changed = false;
    if !e.files.started {
        e.files.started = true;
        s.project.recent = recent::load();
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
                if save_to(e, s, &p)
                    && let Some(t) = then
                {
                    carry_out(e, s, t);
                }
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
    [dialog, autosave_at(e, s)].into_iter().flatten().min()
}

/// "Save first?" pop-up's name.
pub const ASK_POPUP: &str = "files/save-first";

/// A pill button at `rect` (drawn by hand, so its place is known): true when clicked.
fn pill(f: &mut Frame, name: &str, rect: Rect, text: &str, kind: Kind) -> bool {
    use pencilpilot_gpu::Quad;
    use pencilpilot_ui::text::Style;
    use pencilpilot_ui::theme::alpha;
    let t = f.theme();
    let fg = if kind == Kind::Primary {
        t.on_coral
    } else {
        t.ink
    };
    let laid = f.lay_out(text, &Style::body(t.text, fg).weight(600), None);
    let res = f.interact(pencilpilot_ui::ui::id(name), rect);
    let base = match kind {
        Kind::Primary => t.coral,
        Kind::Secondary => t.line,
        Kind::Ghost => alpha(t.line, 0.0),
    };
    let fill = if res.held || res.hovered {
        crate::shell::header::mix(base, t.ink, if res.held { 0.18 } else { 0.08 })
    } else {
        base
    };
    f.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, fill).radius(rect.h / 2.0));
    f.put_text(
        &laid,
        rect.x + (rect.w - laid.width) / 2.0,
        rect.y + (rect.h - laid.height) / 2.0,
    );
    res.clicked
}

/// The "Save first?" pop-up (Save, Don't save, Cancel), centred over `r`, while the
/// project asks it. Escape, or closing it with a click outside, cancels. Its buttons'
/// places go in [`ProjectUi::ask_buttons`].
pub fn ask_popup(f: &mut Frame, r: Rect, s: &mut State) {
    use pencilpilot_ui::text::Style;
    // (its state between passes: 0 not showing, 1 showing, 2 answered: the engine has yet
    // to take the answer)
    let Some(q) = s.project.asking.clone() else {
        if f.value(ASK_POPUP, 0.0) != 0.0 {
            f.set_value(ASK_POPUP, 0.0);
        }
        return;
    };
    let state = f.value(ASK_POPUP, 0.0);
    if state == 2.0 {
        return;
    }
    if !f.is_open(ASK_POPUP) {
        if state == 1.0 {
            // closed by a click outside: cancelled
            f.set_value(ASK_POPUP, 2.0);
            s.actions.push(Action::File(CONFIRM_CANCEL.into()));
            return;
        }
        f.open(ASK_POPUP, 0.0, 0.0);
        f.set_value(ASK_POPUP, 1.0);
    }
    let t = f.theme();
    let (w, h) = (460.0, 168.0);
    let card = Rect::new(r.x + (r.w - w) / 2.0, r.y + r.h * 0.3, w, h);
    // the buttons, right to left along the bottom
    let labels = [
        ("files/ask/save", "Save", Kind::Primary),
        ("files/ask/discard", "Don’t save", Kind::Secondary),
        ("files/ask/cancel", "Cancel", Kind::Ghost),
    ];
    let mut rects = [Rect::default(); 3];
    let mut x = card.right() - 24.0;
    for (i, (_, text, _)) in labels.iter().enumerate() {
        let lw = f
            .lay_out(text, &Style::body(t.text, t.ink).weight(600), None)
            .width
            + 36.0;
        x -= lw;
        rects[i] = Rect::new(x, card.bottom() - 24.0 - 44.0, lw, 44.0);
        x -= 8.0;
    }
    s.project.ask_buttons = rects;
    let mut picked = None;
    f.popup_at(ASK_POPUP, card, 20.0, 24.0, 1.0, |f| {
        f.heading(&q);
        f.label("Your changes are lost if you don’t save them.");
        for (i, (name, text, kind)) in labels.iter().enumerate() {
            if pill(f, name, rects[i], text, *kind) {
                picked = Some([CONFIRM_SAVE, CONFIRM_DISCARD, CONFIRM_CANCEL][i]);
            }
        }
    });
    if f.take_key("Escape", false) {
        picked = Some(CONFIRM_CANCEL);
    }
    if let Some(p) = picked {
        f.set_value(ASK_POPUP, 2.0);
        f.close();
        s.actions.push(Action::File(p.into()));
    }
}
